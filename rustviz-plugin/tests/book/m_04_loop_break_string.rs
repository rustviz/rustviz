fn main() {
    let mut counter = 0;
    let s = loop {
        counter += 1;
        if counter == 3 {
            break String::from("done");
        }
    };
    println!("{} {}", s, counter);
}
