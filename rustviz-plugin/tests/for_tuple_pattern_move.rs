fn main() {
    let pairs = vec![(1, String::from("a")), (2, String::from("b"))];
    for (n, s) in pairs {
        println!("{} {}", n, s);
        break;
    }
    let more = vec![(3, String::from("c"))];
    let it = more.into_iter().enumerate();
    for (i, (n, s)) in it {
        println!("{} {} {}", i, n, s);
    }
    let ids = vec![(1, 2), (3, 4)];
    for (a, b) in ids {
        println!("{} {}", a, b);
    }
}
