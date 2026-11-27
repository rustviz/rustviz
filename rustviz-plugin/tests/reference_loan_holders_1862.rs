struct Holder<'a> {
    view: &'a String,
}

fn sibling(flag: bool, a: &mut String, b: &mut String) {
    let mut r = &mut *a;
    let m = &mut *r;
    if flag {
        r = &mut *b;
        r.clear();
    } else {
        let h = Holder { view: &*m };
        println!("{}", h.view);
    }
}

fn read(_: &String, _: usize) {}

fn unnamed(r: &mut String) {
    let first = &mut *r;
    read(
        &*first,
        {
            let n = 1;
            n + 1
        },
    );
}

fn main() {}

fn two_holders(r: &mut String, flag: bool) {
    let first = &mut *r;
    let mut left = None;
    let mut right = None;
    if flag {
        left = Some(&*first);
    } else {
        right = Some(&*first);
    }
    println!("{left:?} {right:?}");
}
