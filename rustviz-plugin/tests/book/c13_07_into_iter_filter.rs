struct Shoe {
    size: u32,
}

fn shoes_in_size(shoes: Vec<Shoe>, shoe_size: u32) -> Vec<Shoe> {
    shoes.into_iter().filter(|s| s.size == shoe_size).collect()
}

fn main() {
    let shoes = vec![Shoe { size: 10 }, Shoe { size: 13 }];
    let mine = shoes_in_size(shoes, 10);
    println!("{}", mine.len());
}
