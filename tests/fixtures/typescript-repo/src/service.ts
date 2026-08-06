export class Widget {
  build(): number {
    return this.size();
  }

  size(): number {
    return 1;
  }
}

export class Gadget {
  build(): number {
    return 2;
  }
}
