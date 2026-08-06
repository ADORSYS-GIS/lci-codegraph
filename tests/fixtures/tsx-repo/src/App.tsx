import { useCaller } from './hooks';
import { Widget } from './widgets';

export const App = () => {
  const n = useCaller();
  Widget.build();
  return <div>{n}</div>;
};

function runBare(): void {
  // `build` is defined on both Widget and Gadget with no qualifier here — must be
  // dropped as ambiguous, never guessed.
  build();
}
