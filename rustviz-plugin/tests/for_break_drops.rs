// A plain `break` drops the loop's iterator and what the body owns (#740).
fn find(words: Vec<String>) {
    let it = words.into_iter();
    for w in it {
        let copy = w.clone();
        if copy.is_empty() {
            break;
        } else {
            println!("{}", copy);
        }
    }
    println!("done");
}

// A second `for` loop that reuses a binding name binds a fresh variable.
fn twice(a: Vec<i32>, b: Vec<i32>) {
    for n in a {
        println!("{}", n);
    }
    for n in b {
        println!("{}", n);
    }
}

fn main() {
    find(vec![String::from("x")]);
    twice(vec![1], vec![2]);
}
