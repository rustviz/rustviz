fn main() {
    let mut name = String::from("The Rust Book"); // !{ Move(String::from()->name) }
    let serial_num = 1140987; // !{ Bind(serial_num) }
    {
        let rust_book = Book::new(&name, serial_num); // !{ PassByStaticReference(name->Book::new()), Copy(serial_num->Book::new()), Move(Book::new()->rust_book) }
        println!(
            “The name of the book is {}, serial number: {}“, // !{ Lifetime(<FUNC: max>[name{3:15}*NAME* wow wow *DRPT* bow bow][serial_num{7:15}]->[name{8:30}][serial_num{8:22}]) }
            rust_book.name, rust_book.serial_num
        );
    } // !{ GoOutOfScope(rust_book) }
    name = String::from("Behind Borrow Checker"); // !{ Move(String::from()->name) }
    println!(“New name: {}“, name);
} 