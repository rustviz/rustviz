fn main() {
    
    let <tspan data-hash="1">x</tspan> = <tspan class="fn" data-hash="0" hash="4">String::from</tspan>("World");
    
    let <tspan data-hash="2">len_x</tspan> = || <tspan data-hash="1">x</tspan>.len();
    
    let <tspan data-hash="3">print_x</tspan> = || println!("{}", <tspan data-hash="1">x</tspan>);

    println!("{}",<tspan data-hash="2">len_x</tspan>()); // 5
    
    <tspan data-hash="3">print_x</tspan>(); // World 
    
}