package com.example.inventory.service;

public interface StockService {
    Object findBySku(String sku);
    Object updateStock(String sku, Object payload);
    Object reserve(String sku);
    void health();
    Object getAdminStats();
}
