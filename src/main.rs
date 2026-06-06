use rusqlite::{params, Connection};
use std::{
    env::{self},
    fs::File,
    io::{BufReader, Read},
    time::{Instant, UNIX_EPOCH},
};
use walkdir::WalkDir;
use xxhash_rust::xxh3::{self, Xxh3};

// Struct mappata per le colonne del database
struct FileData {
    filepath: String,
    hash: String,
    creation_time: i64,
    modification_time: i64,
    size: i64,
}

// Implementazione della comparazione tra istanze di Filedata
impl PartialEq for FileData {
    fn eq(&self, other: &Self) -> bool {
        self.filepath == other.filepath
            && self.creation_time == other.creation_time
            && self.modification_time == other.modification_time
            && self.size == other.size
    }
}

// Enum per i colori e stili dell'output
#[allow(dead_code)]
enum Color {
    Black,
    Red,
    Green,
    Yellow,
    Blue,
    Purple,
    Cyan,
    White,
}

#[allow(dead_code)]
enum Style {
    Regular,
    Bold,
    Underline,
}

#[allow(dead_code)]
enum Intensity {
    Low,
    High,
}

//
// NOTE, il file deve esistere nella cartella dove viene eseguito il programma
//

fn main() -> Result<(), Box<dyn std::error::Error>> {
    //
    // Recupero gli argomenti del programma
    //

    let instant = Instant::now();

    // Apro la connesione con il database
    let conn = Connection::open("./db.sqlite3")?;

    // Recupero gli argomenti del programma
    let args: Vec<String> = env::args().collect();

    // Verifico se ci sono gli argomenti necessari per il funzionamento
    if let Some(operation) = args.get(1) {
        if let Some(folder) = args.get(2) {
            let folder_trimmed = folder.trim().trim_end_matches("/");
            match operation.as_str() {
                "empty" => empty_db(conn)?,
                "update" => update_db(conn, folder_trimmed)?,
                "check" => check_db(conn)?,
                "prune" => prune_db(conn)?,
                _ => {
                    return Err("ARGOMENTI ERRATI\n{{empty|update|check|prune}} PATH".into());
                }
            }
        } else {
            return Err("CARTELLA MANCANTE\n{{empty|update|check|prune}} PATH".into());
        }
    } else {
        return Err("MANCANTE\n{{empty|update|check|prune}} PATH".into());
    }

    println!("Tempo totale: {:?}", instant.elapsed());
    return Ok(());
}

// Svuota il database in modo approfondito
fn empty_db(conn: Connection) -> Result<(), Box<dyn std::error::Error>> {
    // Creo la tabella se non esiste
    conn.execute(
        "
        CREATE TABLE IF NOT EXISTS Files (
            filepath TEXT PRIMARY KEY,
            hash TEXT NOT NULL,
            creation_time INT NOT NULL,
            modification_time INT NOT NULL,
            size INT NOT NULL
        );",
        (),
    )?;
    conn.execute_batch("DELETE FROM Files; VACUUM;")?;
    return Ok(());
}

// Aggiorno le entrate nel database
fn update_db(mut conn: Connection, folder: &str) -> Result<(), Box<dyn std::error::Error>> {
    // Creo la tabella se non esiste
    conn.execute(
        "
        CREATE TABLE IF NOT EXISTS Files (
            filepath TEXT PRIMARY KEY,
            hash TEXT NOT NULL,
            creation_time INT NOT NULL,
            modification_time INT NOT NULL,
            size INT NOT NULL
        );",
        (),
    )?;

    // Istanzio variabili per le statistiche
    let mut updated: u32 = 0;
    let mut inserted: u32 = 0;
    let mut skipped: u32 = 0;
    let mut errored: u32 = 0;
    // vettori contenenti i percorsi dei file per le statistiche
    let mut inserted_vec: Vec<String> = Vec::new();
    let mut updated_vec: Vec<String> = Vec::new();
    let mut errored_vec: Vec<String> = Vec::new();

    // Recupero il contenuto della cartella e il numero totale dei file
    let folder_content = get_folder_content(folder);
    let file_totali: usize = get_folder_content(folder).count();

    // Recupero le righe del database e le metto in nu vettore
    let file_data_vec_db = db_table_to_vec(&conn)?;

    // Apro la transazione, riduce l'IO
    let transaction = conn.transaction()?;

    {
        // Crea la prepared statement
        let mut statement = transaction.prepare(
            "
            INSERT INTO Files(filepath, hash, creation_time, modification_time, size)
            VALUES (?1, ?2, ?3, ?4, ?5) ON CONFLICT(filepath)
            DO UPDATE
            SET hash = excluded.hash,
                creation_time = excluded.creation_time,
                modification_time = excluded.modification_time,
                size = excluded.size;",
        )?;

        // Scorri tutti i file trovati
        for file in folder_content {
            // Verifico se il file può essere aperto
            if File::open(&file.path()).is_err() {
                errored = errored + 1;
                println!(
                    "{} -> {:?} -> ({}/{})",
                    colored_string(
                        format!("ERRORE LETTURA FILE, FILE NON LEGGIBILE").as_str(),
                        Color::Red,
                        Style::Bold,
                        Intensity::High
                    ),
                    &file.path(),
                    updated + inserted + skipped + errored,
                    file_totali,
                );
                errored_vec.push(file.path().to_str().unwrap().to_string());
                continue;
            }

            let query_time = Instant::now();

            // contiene il file e i suoi metadati reali sul disco
            let file_data_real = generate_file_data_no_hash(file)?;

            // contiene il file e i suoi metadati recuperati dal database
            let file_found_in_db = file_data_vec_db
                .binary_search_by(|f| f.filepath.as_str().cmp(file_data_real.filepath.as_str()));

            // verifico lo stato del file trovato nel db
            match file_found_in_db {
                // CASO FILE TORVATO
                Ok(idx) => {
                    let file_found: &FileData = &file_data_vec_db[idx];
                    // salto il file se i suoi metadati non sono cambiati per accellerare il processo
                    if file_found.creation_time == file_data_real.creation_time
                        && file_found.modification_time == file_data_real.modification_time
                        && file_found.size == file_data_real.size
                    {
                        skipped = skipped + 1;

                        println!(
                            "{} -> {} -> ({}/{}) -> {:?}",
                            colored_string("SKIP", Color::Cyan, Style::Regular, Intensity::Low),
                            file_data_real.filepath,
                            updated + inserted + skipped + errored,
                            file_totali,
                            query_time.elapsed()
                        );
                    } else {
                        updated = updated + 1;
                        // Aggiungo il nuovo hash del file
                        // Eseguo la query preparata
                        statement.execute(params![
                            file_data_real.filepath,
                            genereate_file_checksum(&file_data_real.filepath)?,
                            file_data_real.creation_time,
                            file_data_real.modification_time,
                            file_data_real.size,
                        ])?;

                        // Stampa dati di transazione
                        println!(
                            "{} -> {} -> ({}/{}) -> {:?}",
                            colored_string(
                                "UPDATED",
                                Color::Yellow,
                                Style::Regular,
                                Intensity::Low
                            ),
                            file_data_real.filepath,
                            updated + inserted + skipped + errored,
                            file_totali,
                            query_time.elapsed()
                        );

                        updated_vec.push(file_data_real.filepath);
                    }
                }
                // CASO FILE NON TROVATO
                Err(_) => {
                    inserted = inserted + 1;
                    // Aggiungo il nuovo hash del file
                    // Eseguo la query preparata
                    statement.execute(params![
                        file_data_real.filepath,
                        genereate_file_checksum(&file_data_real.filepath)?,
                        file_data_real.creation_time,
                        file_data_real.modification_time,
                        file_data_real.size,
                    ])?;

                    // Stampa dati di transazione
                    println!(
                        "{} -> {} -> ({}/{}) -> {:?}",
                        colored_string("INSERT", Color::Green, Style::Regular, Intensity::Low),
                        file_data_real.filepath,
                        updated + inserted + skipped + errored,
                        file_totali,
                        query_time.elapsed()
                    );

                    inserted_vec.push(file_data_real.filepath);
                }
            }
        }
    }
    // Confermo la transazione
    transaction.commit()?;

    println!("File inseriti({}):\n{:#?}", inserted, inserted_vec);
    println!("File aggiornati({}):\n{:#?}", updated, updated_vec);
    println!("File con errori({}):\n{:#?}", errored, errored_vec);

    return Ok(());
}

// Verifico i checksum nel db contro i file nel disco
fn check_db(conn: Connection) -> Result<(), Box<dyn std::error::Error>> {
    // Istanzio variabili per le statistiche
    let mut verified: u32 = 0;
    let mut different: u32 = 0;
    let mut errored: u32 = 0;
    let mut missing: u32 = 0;

    let mut different_vec: Vec<String> = Vec::new();
    let mut missing_vec: Vec<String> = Vec::new();
    let mut errored_vec: Vec<String> = Vec::new();

    // Creo la tabella se non esiste
    conn.execute(
        "
        CREATE TABLE IF NOT EXISTS Files (
            filepath TEXT PRIMARY KEY,
            hash TEXT NOT NULL,
            creation_time INT NOT NULL,
            modification_time INT NOT NULL,
            size INT NOT NULL
        );",
        (),
    )?;

    // Recupero le righe del database e le metto in nu vettore
    let file_data_vec_db = db_table_to_vec(&conn)?;
    // prendo il numero dei file nel database
    let file_totali_db = file_data_vec_db.len();

    for file_data in file_data_vec_db {
        // verifico lo stato di esistenza del file
        match std::fs::exists(&file_data.filepath) {
            Ok(exists) => {
                // Se esiste verifico il checksum
                if exists {
                    let checksum_time = Instant::now();
                    if let Ok(checksum_on_disk) = genereate_file_checksum(&file_data.filepath) {
                        // verifico se è uguale l'hash
                        if file_data.hash == checksum_on_disk {
                            verified = verified + 1;
                            println!(
                                "{} -> {} -> ({}/{}) -> {:?}",
                                colored_string("OK", Color::Green, Style::Regular, Intensity::Low),
                                &file_data.filepath,
                                verified + different + missing + errored,
                                file_totali_db,
                                checksum_time.elapsed()
                            );
                        } else {
                            different = different + 1;
                            println!(
                                "{} -> {} -> ({}/{}) -> {:?}",
                                colored_string(
                                    "DIFFERENT",
                                    Color::Yellow,
                                    Style::Regular,
                                    Intensity::Low
                                ),
                                &file_data.filepath,
                                verified + different + missing + errored,
                                file_totali_db,
                                checksum_time.elapsed()
                            );
                            different_vec.push(file_data.filepath);
                        }
                    } else {
                        errored = errored + 1;
                        println!(
                            "{} -> {} -> ({}/{})",
                            colored_string(
                                format!("ERRORE LETTURA FILE, FILE NON LEGGIBILE").as_str(),
                                Color::Red,
                                Style::Bold,
                                Intensity::High
                            ),
                            &file_data.filepath,
                            verified + different + missing + errored,
                            file_totali_db,
                        );
                        errored_vec.push(file_data.filepath);
                    }
                } else {
                    missing = missing + 1;
                    println!(
                        "{} -> {} -> ({}/{})",
                        colored_string(
                            format!("NOT EXISTS").as_str(),
                            Color::Red,
                            Style::Regular,
                            Intensity::Low
                        ),
                        &file_data.filepath,
                        verified + different + missing + errored,
                        file_totali_db,
                    );
                    missing_vec.push(file_data.filepath);
                }
            }
            Err(_) => {
                errored = errored + 1;

                println!(
                    "{} -> {} -> ({}/{})",
                    colored_string(
                        format!("ERRORE RICERCA FILE").as_str(),
                        Color::Red,
                        Style::Bold,
                        Intensity::High
                    ),
                    &file_data.filepath,
                    verified + different + missing + errored,
                    file_totali_db,
                );
                errored_vec.push(file_data.filepath);
            }
        }
    }

    println!("File mancanti({}):\n{:#?}", missing, missing_vec);
    println!("File con errori({}):\n{:#?}", errored, errored_vec);

    return Ok(());
}

fn prune_db(mut conn: Connection) -> Result<(), Box<dyn std::error::Error>> {
    let mut present: u32 = 0;
    let mut removed: u32 = 0;
    let mut errored: u32 = 0;

    let mut removed_vec: Vec<String> = Vec::new();
    let mut errored_vec: Vec<String> = Vec::new();

    // Creo la tabella se non esiste
    conn.execute(
        "
        CREATE TABLE IF NOT EXISTS Files (
            filepath TEXT PRIMARY KEY,
            hash TEXT NOT NULL,
            creation_time INT NOT NULL,
            modification_time INT NOT NULL,
            size INT NOT NULL
        );",
        (),
    )?;

    // Recupero le righe del database e le metto in nu vettore
    let file_data_vec_db = db_table_to_vec(&conn)?;
    // prendo il numero dei file nel database
    let file_totali_db = file_data_vec_db.len();

    // Apro la transazione
    let transaction = conn.transaction()?;
    {
        let mut statement = transaction.prepare(
            "
        DELETE FROM Files WHERE Files.filepath = ?1
        ",
        )?;

        for file_data in file_data_vec_db {
            match std::fs::exists(&file_data.filepath) {
                Ok(exists) => {
                    if !exists {
                        removed = removed + 1;
                        statement.execute(params![file_data.filepath])?;
                        println!(
                            "{} -> {} -> ({}/{})",
                            colored_string(
                                "ASSENTE",
                                Color::Yellow,
                                Style::Regular,
                                Intensity::Low
                            ),
                            &file_data.filepath,
                            removed + present + errored,
                            file_totali_db
                        );
                        removed_vec.push(file_data.filepath);
                    } else {
                        present = present + 1;
                        println!(
                            "{} -> {} -> ({}/{})",
                            colored_string(
                                "PRESENTE",
                                Color::Green,
                                Style::Regular,
                                Intensity::Low
                            ),
                            &file_data.filepath,
                            removed + present + errored,
                            file_totali_db
                        );
                    }
                }
                Err(_) => {
                    errored = errored + 1;
                    println!(
                        "{} -> {} -> ({}/{})",
                        colored_string(
                            format!("ERRORE VERIFICA ESISTENZA FILE").as_str(),
                            Color::Red,
                            Style::Bold,
                            Intensity::High
                        ),
                        &file_data.filepath,
                        removed + present + errored,
                        file_totali_db
                    );
                    errored_vec.push(file_data.filepath);
                }
            }
        }
    }
    transaction.commit()?;

    println!("File rimosssi({}):\n{:#?}", removed, removed_vec);
    println!("File con errori({}):\n{:#?}", errored, errored_vec);

    return Ok(());
}

// Genera un checksum a partire da un file. Propaga gli errori di io::Error
fn genereate_file_checksum(file_path: &str) -> Result<String, std::io::Error> {
    // Prepara l'hasher
    let mut hasher: Xxh3 = xxh3::Xxh3::new();
    // Apri il file
    let file = File::open(file_path)?;
    // Creo il buffer reader
    let mut reader: BufReader<File> = BufReader::new(file);
    // Crea un buffer di 8MiB
    let mut buffer: Vec<u8> = vec![0u8; 8 * 1024 * 1024];
    // Aggiorna il checksum fino alla fine del file
    loop {
        let bytes: usize = reader.read(&mut buffer)?;
        if bytes == 0 {
            break;
        }
        hasher.update(&buffer[..bytes]);
    }

    return Ok(format!("{:X}", hasher.digest128()));
}

// fn generate_file_data(file: walkdir::DirEntry) -> Result<FileData, Box<dyn std::error::Error>> {
//     let temp_filepath = file.path().to_str().unwrap();
//     return Ok(FileData {
//         filepath: temp_filepath.to_owned(),
//         hash: genereate_file_checksum(temp_filepath)?,
//         creation_time: get_created_time_from_file(&file),
//         modification_time: get_modified_time_from_file(&file),
//         size: get_files_size(temp_filepath),
//     });
// }

fn generate_file_data_no_hash(
    file: walkdir::DirEntry,
) -> Result<FileData, Box<dyn std::error::Error>> {
    let temp_filepath = file.path().to_str().unwrap();
    return Ok(FileData {
        filepath: temp_filepath.to_owned(),
        hash: "".to_owned(),
        creation_time: get_created_time_from_file(&file),
        modification_time: get_modified_time_from_file(&file),
        size: get_files_size(&temp_filepath),
    });
}

fn get_files_size(file_path: &str) -> i64 {
    return std::fs::metadata(file_path)
        .unwrap()
        .len()
        .try_into()
        .unwrap();
}

fn get_folder_content(folder: &str) -> impl Iterator<Item = walkdir::DirEntry> {
    return WalkDir::new(&folder)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| !e.path().is_dir())
        .filter(|e| !e.path().starts_with("./checksum-handler"))
        .filter(|e| !e.path().starts_with("./checksum.xxh3"))
        .filter(|e| !e.path().starts_with("./db.sqlite3"));
}

fn db_table_to_vec(conn: &Connection) -> Result<Vec<FileData>, Box<dyn std::error::Error>> {
    let mut statement = conn.prepare("SELECT * FROM Files ORDER BY filepath;")?;

    let mapped = statement.query_map([], |row| {
        Ok(FileData {
            filepath: row.get(0)?,
            hash: row.get(1)?,
            creation_time: row.get(2)?,
            modification_time: row.get(3)?,
            size: row.get(4)?,
        })
    })?;

    let mut rows: Vec<FileData> = Vec::new();

    for item in mapped {
        rows.push(item?);
    }

    Ok(rows)
}

fn get_created_time_from_file(file: &walkdir::DirEntry) -> i64 {
    return file
        .path()
        .metadata()
        .unwrap()
        .created()
        .unwrap()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
        .try_into()
        .unwrap();
}

fn get_modified_time_from_file(file: &walkdir::DirEntry) -> i64 {
    return file
        .path()
        .metadata()
        .unwrap()
        .modified()
        .unwrap()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
        .try_into()
        .unwrap();
}

fn colored_string(line: &str, color: Color, style: Style, intensity: Intensity) -> String {
    let mut color_number: u8;
    let style_number: u8;
    let intensity_offset: u8 = 60;
    let escape_character: String = String::from("\x1b");
    let reset: String = format!("{escape_character}[0m");

    match color {
        Color::Black => color_number = 30,
        Color::Red => color_number = 31,
        Color::Green => color_number = 32,
        Color::Yellow => color_number = 33,
        Color::Blue => color_number = 34,
        Color::Purple => color_number = 35,
        Color::Cyan => color_number = 36,
        Color::White => color_number = 37,
    }

    match style {
        Style::Regular => style_number = 0,
        Style::Bold => style_number = 1,
        Style::Underline => style_number = 4,
    }

    match intensity {
        Intensity::Low => color_number = color_number + 0,
        Intensity::High => color_number = color_number + intensity_offset,
    }

    return format!("{escape_character}[{style_number};{color_number}m{line}{reset}");
}
