function helper() {
  return 1;
}

function caller() {
  return helper();
}

module.exports = { caller };
