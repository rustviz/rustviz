/* --- BEGIN Variable Definitions ---
LifetimeVars &r2; LifetimeVars opt; LifetimeVars c;
--- END Variable Definitions --- */
struct Circle<'i>{
    r: &'i i32,
}
fn main(){
    let r1 = 10;
    let r2 = 9;
    let c = Circle{r: &r1 };
    let opt = c.cmp(&r2); // !{ Lifetime(<STRUCT: Circle::cmp>[c{11:13}][&r2{11:11}]->[opt{11:12}])}
    println!("{} is larger", opt);
}
impl<'i> Circle<'i>{
    fn cmp(&'i self, other: &'i i32) -> &'i i32{
        if self.r > other {self.r}
        else{other}
    }
}