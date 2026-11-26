fn main() {
    let a = 1;
    let mut b = 2;
    let cond = true;
    let h: Option<(&i32, &mut i32)> = if cond { Some((&a, &mut b)) } else { None };
    if let Some((_, m)) = h { *m = 5; }
    b = 3;
}
