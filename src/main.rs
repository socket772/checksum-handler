use std::{
    collections::HashMap,
    env::{self},
    fmt::{self},
    fs::{self, File, OpenOptions},
    io::{self, BufRead, Write},
    path::Path,
    time::{Instant, UNIX_EPOCH},
};
use walkdir::WalkDir;
use xxhash_rust::xxh3::xxh3_128;

#[derive(Clone)]
struct FileMetadata {
    hash: String,          // Risultato
    file_created: String,  // Per accellerare
    file_modified: String, // Per accellerare
}

impl fmt::Display for FileMetadata {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}>{}>{}",
            self.hash, self.file_created, self.file_modified
        )
    }
}

// Enum per i colori e stili dell'output
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

enum Style {
    Regular,
    Bold,
    Underline,
}

enum Intensity {
    Low,
    High,
}

//
// NOTE, il file deve esistere nella cartella dove viene eseguito il programma
//

fn main() {
    //
    // Recupero gli argomenti del programma
    //

    let instant = Instant::now();

    let args: Vec<String> = env::args().collect();
    if args.len() != 3 {
        println!("{{--create|--check}} PATH")
    }

    let folder = args.get(2).unwrap().trim().trim_end_matches("/");

    let operation = args.get(1).unwrap();

    if operation == "--create" {
        create_checksum(folder);
    } else if operation == "--check" {
        check_checksum(folder);
    } else {
        println!("ARGOMENTI ERRATI\n{{--create|--check}} PATH");
    }

    println!("Tempo totale: {:?}", instant.elapsed());
}

fn create_checksum(folder: &str) {
    //
    // Leggi dal file e carica in hasmap_disk
    //
    let mut hashmap_disk: HashMap<String, FileMetadata> = create_hashmap_from_file(folder);

    //
    // Variabili per le statistiche finali
    //
    let mut updated: u32 = 0;
    let mut added: u32 = 0;
    let mut skipped: u32 = 0;

    let folder_content = WalkDir::new(&folder)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| !e.path().is_dir())
        .filter(|e| !e.path().starts_with("./checksum-handler"))
        .filter(|e| !e.path().starts_with("./checksum.xxh3"));

    let file_totali: usize = WalkDir::new(&folder)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| !e.path().is_dir())
        .filter(|e| !e.path().starts_with("./checksum-handler"))
        .filter(|e| !e.path().starts_with("./checksum.xxh3"))
        .count();

    //
    // Ricerca tutti i file e aggiungili confrontali con quelli del file
    // Se non esistono, aggiungili. Se esistono aggiornali
    //

    // let fodler_contents = ;

    for file in folder_content {
        let timenow = Instant::now();
        let file_path_string = file.path().to_str().unwrap();

        if hashmap_disk.contains_key(file_path_string) {
            let file_metadata = hashmap_disk.get(file_path_string).unwrap();
            if is_file_readonly(&file)
                || (file_metadata.file_created == get_created_time_from_file(&file)
                    && file_metadata.file_modified == get_modified_time_from_file(&file))
            {
                skipped = skipped + 1;
                // Cyan
                println!(
                    "{} -> {} -> ({}/{}) -> {:?}",
                    colored_string("SKIP", Color::Cyan, Style::Regular, Intensity::Low),
                    file_path_string,
                    updated + added + skipped,
                    file_totali,
                    timenow.elapsed()
                );
            } else {
                updated = updated + 1;
                let file_metadata = genereate_file_metadata(&file);
                hashmap_disk
                    .entry(file_path_string.to_owned())
                    .or_insert(file_metadata);
                // Purple
                println!(
                    "{} -> {} -> ({}/{}) -> {:?}",
                    colored_string("UPDATE", Color::Yellow, Style::Regular, Intensity::Low),
                    file_path_string,
                    updated + added + skipped,
                    file_totali,
                    timenow.elapsed()
                );
            }
        } else {
            added = added + 1;
            let file_metadata = genereate_file_metadata(&file);
            hashmap_disk.insert(file_path_string.to_owned(), file_metadata.clone());
            // Green
            println!(
                "{} -> {} -> ({}/{}) -> {:?}",
                colored_string("INSERT", Color::Green, Style::Regular, Intensity::Low),
                file_path_string,
                updated + added + skipped,
                file_totali,
                timenow.elapsed()
            );
        }
    }

    //
    // Preparo la stringa da scrivere e scrivo sul file
    //

    let mut file_checksum = OpenOptions::new()
        .write(true)
        .create(true)
        .open(format!("{folder}/checksum.xxh3"))
        .unwrap();
    file_checksum
        .write(hashmap_to_string(hashmap_disk).as_bytes())
        .unwrap();
    println!("Stats: U{updated}, A{added}, S{skipped}");
}

fn check_checksum(folder: &str) {
    println!("Inizio a calcolare il checksum dei file");
    let hashmap_disk = create_hashmap_from_file(folder);
    let file_totali: usize = hashmap_disk.len();
    let mut file_verificati: u32 = 0;

    for file in hashmap_disk {
        let file_data = fs::read(&file.0);
        if file_data.is_err() {
            println!(
                "{}",
                colored_string(
                    format!("READ ERROR (?FILE NOT EXISTS?) -> '{}'", file.0).as_str(),
                    Color::Red,
                    Style::Bold,
                    Intensity::High
                )
            );
            continue;
        }
        let timenow = Instant::now();
        if format!("{:X}", xxh3_128(&file_data.unwrap())) == file.1.hash {
            println!(
                "{} -> {} -> ({}/{}) -> {:?}",
                colored_string("OK", Color::Green, Style::Regular, Intensity::Low),
                &file.0,
                file_verificati,
                file_totali,
                timenow.elapsed()
            )
        } else {
            println!(
                "{} -> {} -> ({}/{}) -> {:?}",
                colored_string("FAIL", Color::Red, Style::Regular, Intensity::Low),
                &file.0,
                file_verificati,
                file_totali,
                timenow.elapsed()
            )
        }
        file_verificati = file_verificati + 1;
    }
}

fn get_modified_time_from_file(file: &walkdir::DirEntry) -> String {
    return format!(
        "{:X}",
        file.path()
            .metadata()
            .unwrap()
            .modified()
            .unwrap()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs()
    );
}

fn get_created_time_from_file(file: &walkdir::DirEntry) -> String {
    return format!(
        "{:X}",
        file.path()
            .metadata()
            .unwrap()
            .created()
            .unwrap()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs()
    );
}

fn is_file_readonly(file: &walkdir::DirEntry) -> bool {
    return file.metadata().unwrap().permissions().readonly();
}

fn read_checksum_file(folder: &str) -> io::Result<io::Lines<io::BufReader<File>>> {
    if !Path::new(&format!("{folder}/checksum.xxh3")).exists() {
        let temp_file_create = File::create(format!("{folder}/checksum.xxh3"));
        drop(temp_file_create)
    }
    let file = File::open(format!("{folder}/checksum.xxh3")).unwrap();
    Ok(io::BufReader::new(file).lines())
}

fn genereate_file_metadata(file: &walkdir::DirEntry) -> FileMetadata {
    return FileMetadata {
        hash: format!("{:X}", xxh3_128(&fs::read(file.path()).unwrap())),
        file_created: get_created_time_from_file(file),
        file_modified: get_modified_time_from_file(file),
    };
}

fn hashmap_to_string(hashmap: HashMap<String, FileMetadata>) -> String {
    let mut stringa_finale = String::new();
    for entry in hashmap {
        stringa_finale.push_str(format!("{}>{}\n", entry.0, entry.1).as_str());
    }
    return stringa_finale;
}

fn create_hashmap_from_file(folder: &str) -> HashMap<String, FileMetadata> {
    //
    // Leggi dal file e carica in hasmap_disk
    //
    let mut hashmap: HashMap<String, FileMetadata> = HashMap::new();
    // let timenow = Instant::now();
    let reader = read_checksum_file(folder).unwrap();
    for line in reader {
        if line.is_err() {
        } else {
            let line_decoded: Vec<String> =
                line.unwrap().split(">").map(|x| x.to_string()).collect();
            println!(
                "{} -> {}",
                colored_string(
                    "DECODED FROM FILE",
                    Color::Purple,
                    Style::Regular,
                    Intensity::Low
                ),
                line_decoded[0]
            );
            let _ = hashmap.insert(
                line_decoded[0].clone(),
                FileMetadata {
                    hash: line_decoded[1].clone(),
                    file_created: line_decoded[2].clone(),
                    file_modified: line_decoded[3].clone(),
                },
            );
        }
    }

    return hashmap;
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
