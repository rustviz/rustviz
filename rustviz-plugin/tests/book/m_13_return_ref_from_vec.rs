fn first<'a>(v: &'a Vec<String>) -> &'a String {
    &v[0]
}

fn main() {
    let v = vec![String::from("a")];
    let f = first(&v);
    println!("{}", f);
}
