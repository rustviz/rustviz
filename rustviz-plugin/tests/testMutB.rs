fn add_exclamation(s: &mut String) {
    s.push_str("!");
}

fn main() {
    let mut greeting = String::from("Hello");
    {
        let r1 = &mut greeting;
        add_exclamation(r1);
    }
    println!("{}", greeting);
}