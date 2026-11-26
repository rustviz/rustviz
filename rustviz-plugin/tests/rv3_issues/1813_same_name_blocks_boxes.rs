fn main() {
    { let b = Box::new(String::from("first")); drop(*b); }
    { let b = Box::new(String::from("second")); println!("{}", b.len()); }
}
