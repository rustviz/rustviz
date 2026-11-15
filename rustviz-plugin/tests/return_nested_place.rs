// A field of an element returned (#1065): `pts[0].x` copies that one
// field out; the array, Vec or static it comes from keeps all of it.
// `unsafe { &mut TOTAL }` hands the caller a mutable reference to the
// static (the `unsafe` block only wraps the place). `&mut ps[0].x` /
// `&rows[1].y` with `ps` / `rows` a reference reborrow through it, so
// its borrow goes on with the returned reference. In `through_refs` the
// element is itself a reference, so `rs[0].x` isn't data rs holds.
#![allow(static_mut_refs)]
struct Pt { x: i32, y: i32 }
struct In { n: u8 }
struct Out { inner: In }
struct W { n: u8, name: String }
static PTS: [Pt; 2] = [Pt { x: 1, y: 2 }, Pt { x: 3, y: 4 }];
static mut TOTAL: u32 = 0;
static mut GRID: [[u8; 2]; 2] = [[0; 2]; 2];

fn first_x() -> i32 {
    let pts = [Pt { x: 1, y: 2 }];
    pts[0].x
}

fn copy_x() -> i32 {
    let qs = [Pt { x: 1, y: 2 }];
    let a = qs[0].x;
    a
}

fn first_n() -> u8 {
    let ws = vec![W { n: 1, name: String::from("w") }];
    println!("{}", ws[0].name);
    ws[0].n
}

fn inner_n() -> u8 {
    let os = [Out { inner: In { n: 3 } }];
    os[0].inner.n
}

fn first_y(ps: &[Pt]) -> i32 {
    ps[0].y
}

fn static_y() -> i32 {
    PTS[1].y
}

fn total() -> &'static mut u32 {
    unsafe { &mut TOTAL }
}

fn cell() -> &'static mut u8 {
    unsafe { &mut GRID[1][0] }
}

fn cell_value() -> u8 {
    unsafe { GRID[1][0] }
}

fn x_of(ps: &mut Vec<Pt>) -> &mut i32 {
    &mut ps[0].x
}

fn y_of(rows: &[Pt]) -> &i32 {
    &rows[1].y
}

fn through_refs(rs: Vec<&Pt>) -> i32 {
    rs[0].x
}

fn main() {
    let mut ps = vec![Pt { x: 5, y: 6 }, Pt { x: 7, y: 8 }];
    *total() += 1;
    *cell() += 1;
    let x = x_of(&mut ps);
    *x += 1;
    let y = y_of(&ps);
    println!("{} {} {} {} {} {} {} {}", first_x(), copy_x(), first_n(), inner_n(), first_y(&ps), static_y(), cell_value(), y);
    println!("{}", through_refs(vec![&ps[0]]));
}
