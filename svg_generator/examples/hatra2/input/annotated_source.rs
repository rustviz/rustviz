<tspan fill="#AAA">1</tspan>  fn main(){
<tspan fill="#AAA">2</tspan>      let mut <tspan data-hash="1">s</tspan> = <tspan class="fn" data-hash="0" hash="5">String::from</tspan>("hello");
<tspan fill="#AAA">3</tspan>  
<tspan fill="#AAA">4</tspan>      let <tspan data-hash="2">r1</tspan> = <tspan data-hash="1">&amp;s</tspan>;
<tspan fill="#AAA">5</tspan>      let <tspan data-hash="3">r2</tspan> = <tspan data-hash="1">&amp;s</tspan>;
<tspan fill="#AAA">6</tspan>      assert!(compare_strings(<tspan data-hash="2">r1</tspan>, <tspan data-hash="3">r2</tspan>));
<tspan fill="#AAA">7</tspan>  
<tspan fill="#AAA">8</tspan>      let <tspan data-hash="4">r3</tspan> = <tspan data-hash="1">&amp;mut s</tspan>;
<tspan fill="#AAA">9</tspan>      clear_string(<tspan data-hash="4">r3</tspan>);
<tspan fill="#AAA">10</tspan> }