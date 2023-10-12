use std::io::{self, prelude::*};
use std::fs::File;
use std::path::Path;

/* File handling helper functions. */

// Returns: a result object that we can match to OK(), and then use .lines() on.
pub fn read_file<P>(file_path: P) -> io::Result<io::BufReader<File>>
where
    P: AsRef<Path>,
{
    let file = File::open(file_path)?;
    Ok(io::BufReader::new(file))
}

pub fn read_file_to_string<P>(file_path: P) -> io::Result<String>
where
    P: AsRef<Path>,
{
    let mut string = String::new();
    if let Ok(mut buf) = read_file(file_path) {
        buf.read_to_string(&mut string)?;
    }
    Ok(string.to_owned())
}

// Returns: a result object that we can match to OK(), and then use .lines() on.
pub fn read_lines<P>(file_path: P) -> io::Result<io::Lines<io::BufReader<File>>>
where
    P: AsRef<Path>,
{
    let file = File::open(file_path)?;
    Ok(io::BufReader::new(file).lines())
}

/* First Create the file, then write content into it.
   Arguments:
        file_path:  any object that can be converted to a path
                Notable examples including String, str, Path, PathBuf, OsString.
        content:    a String that needs to be written.
*/
pub fn create_and_write_to_file<P>(content: &String, file_path: P)
where
    P: AsRef<Path>,
{
    let display = file_path.as_ref().display();

    // Open a file in write-only mode, returns `io::Result<File>`
    let mut file = match File::create(file_path.as_ref()) {
        Err(why) => panic!("couldn't create {}: {}", display, why),
        Ok(file) => file,
    };

    // Write the content string to `file`, returns `io::Result<()>`
    match file.write_all(content.as_bytes()) {
        Err(why) => panic!("couldn't write to {}: {}", display, why),
        Ok(_) => println!("successfully wrote to {}", display),
    }
}
