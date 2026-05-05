fn main() {
    let animals = vec!["dog", "cat", "rabbit"];

    for animal in &animals {
        println!("Animal: {}", animal);
    }
}