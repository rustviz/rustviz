// Same as match_ref_param_reborrow, through a `&mut` parameter (#372).

fn bump(o: &mut Option<String>) {
    match o {
        Some(s) => {
            s.push('!');
        }
        None => {}
    }
}

fn main() {
    let mut x = Some(String::from("hi"));
    bump(&mut x);
    println!("{:?}", x);
}
