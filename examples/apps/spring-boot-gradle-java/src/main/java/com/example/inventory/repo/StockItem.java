package com.example.inventory.repo;

import org.springframework.data.mongodb.core.mapping.Document;

@Document(collection = "stock_items")
public class StockItem {
    private String id;
    private String sku;
    private Integer quantity;

    public String getId() {
        return id;
    }

    public String getSku() {
        return sku;
    }

    public Integer getQuantity() {
        return quantity;
    }
}
