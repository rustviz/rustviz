/* --- BEGIN Variable Definitions ---
LifetimeVars &r2; LifetimeVars opt; LifetimeVars c;
--- END Variable Definitions --- */
struct Circle<'i>{
    r: &'i i32,
}

fn main(){
    let r1 = 10;
    let r2 = 9;
    let c = Circle::new(&r1);
    let opt = c.cmp(&r2); // !{ Lifetime(<STRUCT: Circle::cmp>[c{11:14}][&r2{12:12}]->[opt{12:13}])}
    println!("{} is larger", opt);
}



impl<'i> Circle<'i>{
    fn new(_r: &'i i32) -> Circle {
        Circle{r: _r}
    }
}

impl<'i> Circle<'i>{
    fn cmp(&'i self, other: &'i i32) -> &'i i32{
        if self.r > other{
            self.r
        }
        else{
            other
        }
    }
}