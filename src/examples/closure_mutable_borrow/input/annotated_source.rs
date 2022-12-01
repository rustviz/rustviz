fn main(){
    let mut <tspan data-hash="1">x</tspan> = <tspan class="fn" data-hash="0" hash="4">String::from</tspan>("Hello, ");
    
    let mut <tspan data-hash="2">f</tspan> = |y:&amp;String| <tspan data-hash="1">x</tspan>.push_str(y);
    
    let <tspan data-hash="3">world</tspan> = <tspan class="fn" data-hash="0" hash="4">String::from</tspan>("World");
    
    <tspan data-hash="2">f</tspan>(&amp; <tspan data-hash="3">world</tspan>);
    
    println!("{}",<tspan data-hash="1">x</tspan>); //prints Hello, World 
}