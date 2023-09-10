struct Circle<'i>{
    r: &'i i32,
}

fn main(){
    let r1 = 10;
    let r2 = 9;
    let <tspan data-hash="1">c</tspan> = Circle::new(&r1);
    let <tspan data-hash="3">opt</tspan> = <tspan data-hash="1">c</tspan>.cmp(<tspan data-hash="2">&r2</tspan>);
    println!("{} is larger", <tspan data-hash="3">opt</tspan>);
}

impl<'i> Circle<'i>{
    fn new(_r: &'i i32) -> Circle {
        Circle{r: _r}
    }
}

impl<'i> Circle<'i>{
    fn cmp(&self, other: &'i i32) -> &'i i32{
        if self.r > other{
            self.r
        }
        else{
            other
        }
    }
}