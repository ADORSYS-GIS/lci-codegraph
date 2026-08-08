from dataclasses import dataclass

@dataclass
class Product:
    sku: str
    name: str
    price: float

class ProductRepository:
    def __init__(self):
        self.products = {}

    def find_by_sku(self, sku: str):
        return self.products.get(sku)

    def find_all(self):
        return list(self.products.values())

    def insert(self, product: dict):
        p = Product(**product)
        self.products[p.sku] = p
        return p
