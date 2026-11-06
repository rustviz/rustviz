fn show(n: usize) {
    println!("{}", n);
}

fn main() {
    let names = [String::from("a"), String::from("b")];
    show(names.len());

    let first = String::from("c");
    let second = String::from("d");
    let moved = [first, second];
    show(moved.len());

    let mut again = [String::from("e"), String::from("f")];
    show(again.len());
    again = [String::from("g"), String::from("h")];
    show(again.len());

    let gone = [String::from("i"), String::from("j")];
    drop(gone);

    let maybe = [String::from("k"), String::from("l")];
    if names.len() > 5 {
        drop(maybe);
    }

    let nums = [1, 2, 3];
    show(nums.len());
}
