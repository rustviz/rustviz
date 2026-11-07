// `s` reborrows through the parameter `o` (it points into `*o`), so
// the borrow `o` holds can't end before `s`'s does (#372). It used to
// be drawn ending at the `match`, on line 6.

fn show(o: &Option<String>) {
    match o {
        Some(s) => {
            println!("{}", s);
        }
        None => {}
    }
}

fn main() {
    let x = Some(String::from("hi"));
    show(&x);
}
