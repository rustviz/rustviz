/* --- BEGIN Variable Definitions ---
LifetimeVars &name; LifetimeVars serial_num; LifetimeVars rust_book;
--- END Variable Definitions --- */
struct Book<'a>{
    name: &'a String,
    serial_num: i32
}

fn main(){
    let mut name = String::from("The Rust Book");
    let serial_num = 1140987;
    {
        let rust_book = Book::new(&name, serial_num); // !{ Lifetime(<STRUCT: Book::new>[&name{10:10}*CRPT* temporary reference created on the fly. Begins and drops on the same line *DRPT*temporary reference created on the fly. Begins and drops on the same line][serial_num{8:15}]->[rust_book{10:12}])}
        println!("The name of the book is {}", rust_book.name);
    }
    name = String::from("Behind Borrow Checker");
    println!("New name: {}",name);
}

impl<'a> Book<'a>{
    fn new(_name: &'a String, _serial_num: i32) -> Book<'a>{
        Book { name: _name, serial_num: _serial_num }
    }
}

