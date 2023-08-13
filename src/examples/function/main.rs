/* --- BEGIN Variable Definitions ---
Owner name; Owner serial_num; Owner rust_book;
Function String::from(); Function Book::new(); Function String::from();
--- END Variable Definitions --- */
fn main() {
    let mut name = String::from("The Rust Book"); // !{ Move(String::from()->name) }
    let serial_num = 1140987; // !{ Bind(serial_num) }
    {
        let rust_book = Book::new(&name, serial_num); 
        println!(
            “The name of the book is {}, serial number: {}“, 
            rust_book.name, rust_book.serial_num
        );
    } // !{ GoOutOfScope(rust_book) }
    name = String::from("Behind Borrow Checker"); // !{ Move(String::from()->name) }
    println!(“New name: {}“, name);
} // !{ GoOutOfScope(name), GoOutOfScope(serial_num) }