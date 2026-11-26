fn main() {
    { let a = [String::new(), String::new()]; let [s, _] = a; drop(s); }
    { let a = [String::new(), String::new()]; println!("{}", a.len()); }
}
