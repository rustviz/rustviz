<tspan fill="#AAA">1</tspan>  fn main() {
<tspan fill="#AAA">2</tspan>      let <tspan data-hash="1">s</tspan> = <tspan class="fn" data-hash="0" hash="5">String::from</tspan>("hello");
<tspan fill="#AAA">3</tspan>      <tspan class="fn" data-hash="0" hash="6">takes_ownership</tspan>(<tspan data-hash="1">s</tspan>);
<tspan fill="#AAA">4</tspan>      let mut <tspan data-hash="2">x</tspan> = 5;
<tspan fill="#AAA">5</tspan>      let <tspan data-hash="3">y</tspan> = x;
<tspan fill="#AAA">6</tspan>      <tspan data-hash="2">x</tspan> = 6;
<tspan fill="#AAA">7</tspan>  }
<tspan fill="#AAA">8</tspan>  
<tspan fill="#AAA">9</tspan>  fn <tspan class="fn" data-hash="0" hash="6">takes_ownership</tspan>(<tspan data-hash="4">some_string</tspan>: String) {
<tspan fill="#AAA">10</tspan>     <tspan class="fn" data-hash="0" hash="8">println!</tspan>("{}", <tspan data-hash="4">some_string</tspan>);
<tspan fill="#AAA">11</tspan> }