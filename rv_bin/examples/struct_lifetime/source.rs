struct Excerpt<'a> {
    p: &'a str,
}

fn some_function() {
    let n = String::from("Ok. I'm fine.");
    let first = n.split('.').next().expect("Could not find a '.'");
    let i = Excerpt {
        p: first,
    };
    println!("{}", first);
    // 'i' cannot be returned be returned
    // because the struct outlives 'n'
}

fn main() {
    some_function();
}