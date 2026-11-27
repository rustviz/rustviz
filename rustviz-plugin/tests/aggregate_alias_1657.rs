type Alias<'a> = Option<(&'a i32,)>;

fn main() {
    let x = 0;
    let aliased: Alias = Alias::Some((&x,));
    println!("{aliased:?}");
}

enum Entry<'a> {
    Case((&'a i32, &'a mut i32)),
}
type Entries<'a> = Entry<'a>;

fn variant() {
    let mut x = 1;
    let mut y = 2;
    let a = &mut x;
    let b = &mut y;
    let h = Entries::Case((a, b));
    let _ = *a;
    std::hint::black_box(h);
    *b = 3;
}
