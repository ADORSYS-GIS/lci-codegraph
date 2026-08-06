class Widget {
  build() {
    return this.size();
  }

  size() {
    return 1;
  }
}

class Gadget {
  build() {
    return 2;
  }
}

module.exports = { Widget, Gadget };
