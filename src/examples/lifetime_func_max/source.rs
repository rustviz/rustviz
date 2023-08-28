fn main(){
    let a = 10;
    let b = 6;
    let r: &i32;
    {
        let x: &i32 = &a;
        let y: &i32 = &b;
        r = max(x,y);
    }
    println!("r is {}",r);
}

fn max<'a>(x: &'a i32, y: &'a i32) -> &'a i32{
    if x >= y{
        x
    }
    else{
        y
    }
}