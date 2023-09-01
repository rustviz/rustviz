struct Book<'a>{
    name: &'a String,
    serial_num: i32
}

fn main(){
    let mut  name = String::from("The Rust Book");
    let serial_num = 1140987;
    {
        let <tspan data-hash="3">rust_book</tspan> = Book::new(<tspan data-hash="1">&name</tspan>, serial_num);
        println!("The name of the book is {}", <tspan data-hash="3">rust_book.name</tspan>);
    }
    name = String::from("Behind Borrow Checker");
    println!("New name: {}",name);
}

impl<'a> Book<'a>{
    fn new(_name: &'a String, _serial_num: i32) -> Book<'a>{
        Book { name: _name, serial_num: _serial_num }
    }
}