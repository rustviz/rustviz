struct Book<'a>{
    name: &'a String,
    serial_num: i32
}

impl<'a> Book<'a>{
    fn new(_name: &'a String, _serial_num: i32) -> Book<'a>{
        Book { name: _name, serial_num: _serial_num }
    }
}

fn main() { // 1
    let mut name = String::from("The Rust Book"); // 2
    let num_id = 110923; // 3
    { // 4
        /* !{ Lifetime@Func(max:'a)(x[3:5];y[4:6]->r[3;6]) } */  // 5
        let rust_book = Book::new(&name, num_id);   // 6
        println!("The name of the book is {}",rust_book.name);  // 7
    }   // 8
    name = String::from("Behind Borrow Checker");   // 9
    println!("New name: {}",name);  // 10
}