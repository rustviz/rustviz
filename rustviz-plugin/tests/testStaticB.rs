fn print_str(s: &str) {
    println!("{}", s);
}

fn main() {
    let my_string = String::from("Hello, world!");
    let my_str: &str = &my_string;
    print_str(my_str);
}