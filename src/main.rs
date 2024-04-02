use std::fs::File;
use std::io::{self, Read};

fn main() -> io::Result<()> {
    // Open the file in read-only mode
    let mut file = File::open("/Users/Owen/Documents/GitHub/ase-project/SOFA-data/012.sofa")?;

    // Read the contents of the file into a buffer
    let mut buffer = String::new();
    file.read_to_string(&mut buffer)?;

    // Print the contents of the file
    println!("{}", buffer);

    Ok(())
}
