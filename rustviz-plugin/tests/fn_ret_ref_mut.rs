fn first_mut(v: &mut Vec<String>) -> &mut String {
    &mut v[0]
}

fn main() {
    let mut v = vec![String::from("a")];
    let x = first_mut(&mut v);
    x.push('!');
    println!("{}", x);
    v.push(String::from("b"));
    println!("{:?}", v);
}
