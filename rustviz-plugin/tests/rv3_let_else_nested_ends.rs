fn example(mut v: Vec<String>, c: bool) {
    let p = v.first();
    let Some(x) = p else {
        if c {
            println!("{p:?}");
            v.push(String::new());
        } else {
            let r = &v;
            println!("{r:?} {p:?}");
        }
        return;
    };
    println!("{x}");
}

fn main() {}
