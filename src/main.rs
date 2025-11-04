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

    let mut folder = args.get(2).unwrap().trim();

    if folder != "./" {
        folder = folder.trim_end_matches("/");
    }

    println!("Cartella selezionata: {}", folder);

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
    let mut hashmap_disk: HashMap<String, FileMetadata> = HashMap::new();
    // let timenow = Instant::now();
    let reader = read_lines(folder).unwrap();
    for line in reader {
        if line.is_err() {
        } else {
            let line_decoded: Vec<String> =
                line.unwrap().split(">").map(|x| x.to_string()).collect();
            // println!("{:#?}", line_decoded);
            let result_hashmap_insert = hashmap_disk.insert(
                line_decoded[0].clone(),
                FileMetadata {
                    hash: line_decoded[1].clone(),
                    file_created: line_decoded[2].clone(),
                    file_modified: line_decoded[3].clone(),
                },
            );
            if result_hashmap_insert.is_some() {
                // println!("{}", result_hashmap_insert.unwrap())
            }
        }
    }

    // println!("Hashmap insert: {:?}", timenow.elapsed());

    //
    // Ricerca tutti i file e aggiungili confrontali con quelli del file
    // Se non esistono, aggiungili. Se esistono aggiornali
    //

    let mut updated: u32 = 0;
    let mut inserted: u32 = 0;
    let mut skipped: u32 = 0;

    for file in WalkDir::new(&folder).into_iter().filter_map(|e| e.ok()) {
        if file.path().is_dir()
            || file.path().ends_with("checksum-handler")
            || file.path().ends_with("checksum.xxh3")
        {
            continue;
        }

        let _timenow = Instant::now();
        let file_path_string = file.path().to_str().unwrap();

        if hashmap_disk.contains_key(file_path_string) {
            let file_metadata = hashmap_disk.get(file_path_string).unwrap();
            if is_file_readonly(&file)
                || (file_metadata.file_created == get_created_time_from_file(&file)
                    && file_metadata.file_modified == get_modified_time_from_file(&file))
            {
                skipped = skipped + 1;
                // println!("Skip ({}): {:?}", file_path_string, _timenow.elapsed());
            } else {
                updated = updated + 1;
                let file_metadata = genereate_file_metadata(&file);
                hashmap_disk
                    .entry(file_path_string.to_owned())
                    .or_insert(file_metadata);
                // println!("Update ({}): {:?}", file_path_string, _timenow.elapsed());
            }
        } else {
            inserted = inserted + 1;
            let file_metadata = genereate_file_metadata(&file);
            hashmap_disk.insert(file_path_string.to_owned(), file_metadata.clone());
            // println!("Insert: ({:?}): {}", file_path_string, _timenow.elapsed());
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
    let timenow = Instant::now();
    file_checksum
        .write(hashmap_to_string(hashmap_disk).as_bytes())
        .unwrap();
    println!("Hashmap scrivi in file: {:?}", timenow.elapsed());
    println!("Stats: U{updated}, I{inserted}, S{skipped}");
}

fn check_checksum(folder: &str) {
    todo!("programma il controllo del checksum");
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

fn read_lines(folder: &str) -> io::Result<io::Lines<io::BufReader<File>>> {
    if !Path::new(&format!("{folder}/checksum.xxh3")).exists() {
        let temp_file_create = File::create(format!("{folder}/checksum.xxh3"));
        drop(temp_file_create)
    }
    let file = File::open(format!("{}/checksum.xxh3", folder)).unwrap();
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
