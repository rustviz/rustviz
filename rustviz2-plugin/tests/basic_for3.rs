fn main() {
    let animals = vec!["dog", "cat", "rabbit"];

    for (index, animal) in animals.iter().enumerate() {
        println!("Index: {}, Animal: {}", index, animal);
    }
}