export function helper(): number {
  return 1;
}

export function caller(): number {
  return helper();
}
