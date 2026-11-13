use std::io;
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
