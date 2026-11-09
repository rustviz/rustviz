fn parse(s: &str) -> Result<u32, String> {
    s.parse::<u32>().map_err(|e| e.to_string())
}

fn total(words: Vec<String>) -> Result<u32, String> {
    let it = words.into_iter();
    let mut n = 0;
    for w in it {
        n += parse(&w)?;
    }
    Ok(n)
}

fn first(words: Vec<String>) -> usize {
    let it = words.into_iter();
    for w in it {
        if w.is_empty() {
            panic!("empty");
        } else {
            println!("{}", w);
        }
    }
    0
}

fn main() {
    let _ = total(vec![String::from("1")]);
    first(vec![String::from("a")]);
}
