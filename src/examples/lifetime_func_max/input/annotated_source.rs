fn main(){
    let a = 10;
    let b = 6;
    let <tspan data-hash="3">r</tspan>: &i32;
    {
        let <tspan data-hash="1">x</tspan>: &i32 = &a;
        let <tspan data-hash="2">y</tspan>: &i32 = &b;
        <tspan data-hash="3">r</tspan> = max(<tspan data-hash="1">x</tspan>, <tspan data-hash="2">y</tspan>);
    }
    println!("r is {}",<tspan data-hash="3">r</tspan>);
}

fn max<'a>(x: &'a i32, y: &'a i32) -> &'a i32{
    if x >= y{
        x
    }
    else{
        y
    }
}

