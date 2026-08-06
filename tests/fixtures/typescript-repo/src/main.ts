import { caller } from './util';
import { Widget } from './service';

function run(): void {
  caller();
  Widget.build();
}

function runBare(): void {
  // `build` is defined on both Widget and Gadget with no qualifier here — must be
  // dropped as ambiguous, never guessed.
  build();
}
