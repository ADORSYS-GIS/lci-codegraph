export function useThing(): number {
  return 1;
}

export function useCaller(): number {
  return useThing();
}
