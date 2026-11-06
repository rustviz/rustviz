use std::ops::Neg;

#[derive(Debug)]
struct Wave { samples: Vec<i32> }

impl Neg for Wave {
    type Output = Wave;
    fn neg(self) -> Wave {
        Wave { samples: self.samples.iter().map(|s| -s).collect() }
    }
}

struct Point { x: i32, y: i32 }

fn main() {
    let x = 2;
    let b = true;
    let n = -x;
    let m = !b;
    let p = Point { x: 1, y: 2 };
    let q = -p.y;
    let r = &x;
    let k = -*r;
    let lit = -5;
    let wave = Wave { samples: vec![1, 2] };
    let flipped = -wave;
    println!("{} {} {} {} {} {} {} {:?}", n, m, p.x, q, k, r, lit, flipped);
}
