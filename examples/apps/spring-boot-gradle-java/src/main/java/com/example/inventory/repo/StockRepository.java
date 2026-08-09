package com.example.inventory.repo;

import org.springframework.data.mongodb.repository.MongoRepository;
import org.springframework.stereotype.Repository;

@Repository
public interface StockRepository extends MongoRepository<StockItem, String> {
    Object findBySkuIgnoreCase(String sku);
}
