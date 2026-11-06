fn main() {
    let v = vec![String::from("a"), String::from("b")];
    for s in v {
        println!("{}", s);
        break;
    }
    let z = vec![String::from("c"), String::from("d")];
    let it3 = z.into_iter();
    for s in it3 {
        println!("{}", s);
    }
    let n = vec![1, 2];
    for i in n {
        println!("{}", i);
    }
    let q = vec![String::from("x")];
    for s in &q {
        println!("{}", s);
    }
    let it = q.iter();
    for s in it {
        println!("{}", s);
    }
    println!("{}", first_long(vec![String::from("abcd")]));
}

fn first_long(w: Vec<String>) -> usize {
    let words = w.into_iter();
    for word in words {
        if word.len() > 3 {
            return word.len();
        } else {
            println!("{}", word);
        }
    }
    0
}
