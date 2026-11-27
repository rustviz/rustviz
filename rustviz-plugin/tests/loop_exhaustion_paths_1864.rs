// #1864: exhaustion bypasses all of the body's arms.
fn wildcard(xs: &[u32]) -> usize {
    let mut n = 0;
    for _ in xs {
        n += 1;
        if n == 2 {
            break;
        } else {
            continue;
        }
    }
    n
}

fn sibling(xs: &[u32], flag: bool) -> usize {
    let mut n = 0;
    if flag {
        for x in xs {
            n += 1;
            if *x == 0 {
                break;
            } else {
                continue;
            }
        }
    } else {
        n += xs.len();
    }
    n
}

fn two_breaks(xs: &[u32]) -> usize {
    let mut n = 0;
    for x in xs {
        if *x == 0 {
            n += 1;
            break;
        } else {
            n += 2;
            break;
        }
    }
    n
}

fn main() {
    assert_eq!(wildcard(&[]), 0);
    assert_eq!(wildcard(&[1]), 1);
    assert_eq!(wildcard(&[1, 2]), 2);
    assert_eq!(sibling(&[1], true), 1);
    assert_eq!(sibling(&[1, 2], false), 2);
    assert_eq!(two_breaks(&[]), 0);
    assert_eq!(two_breaks(&[0]), 1);
    assert_eq!(two_breaks(&[1]), 2);
}
