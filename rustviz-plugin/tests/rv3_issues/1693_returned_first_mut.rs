fn pick<'a>(a: &'a mut Vec<i32>, b: &'a mut Vec<i32>, c: bool, d: bool) -> Option<&'a mut i32> {
    if c {
        if d {
            a.first_mut()
        } else {
            b.first_mut()
        }
    } else {
        None
    }
}
