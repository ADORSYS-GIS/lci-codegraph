from .repository import ProductRepository

class ProductService:
    def __init__(self):
        self.repo = ProductRepository()

    def get(self, sku: str):
        return self.repo.find_by_sku(sku)

    def list_all(self):
        return self.repo.find_all()

    def create(self, product: dict):
        return self.repo.insert(product)
