package com.example.inventory.service;

import com.example.inventory.repo.StockRepository;
import org.springframework.stereotype.Service;

@Service
public class StockServiceImpl implements StockService {
    private final StockRepository stockRepository;

    public StockServiceImpl(StockRepository stockRepository) {
        this.stockRepository = stockRepository;
    }

    @Override
    public Object findBySku(String sku) {
        return stockRepository.findBySkuIgnoreCase(sku);
    }

    @Override
    public Object updateStock(String sku, Object payload) {
        return stockRepository.findBySkuIgnoreCase(sku);
    }

    @Override
    public Object reserve(String sku) {
        return stockRepository.findById(sku);
    }

    @Override
    public void health() {
        stockRepository.count();
    }

    @Override
    public Object getAdminStats() {
        return stockRepository.findAll();
    }
}
