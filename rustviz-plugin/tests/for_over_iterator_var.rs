fn main() {
    let mut v = vec![1, 2];
    let it = v.iter_mut();
    for x in it {
        *x += 1;
    }
    v.push(3);
    let w = vec![String::from("a")];
    let it2 = w.iter();
    for s in it2 {
        println!("{}", s);
    }
    for s in w {
        println!("{}", s);
    }
}
