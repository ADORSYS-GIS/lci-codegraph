from fastapi import FastAPI
from .service import ProductService

app = FastAPI()
service = ProductService()

@app.get("/products/{sku}")
async def get_product(sku: str):
    return service.get(sku)

@app.get("/products")
async def list_products():
    return service.list_all()

@app.post("/products")
async def create_product(product: dict):
    return service.create(product)
