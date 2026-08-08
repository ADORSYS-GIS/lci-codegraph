package com.example.inventory.api;

import com.example.inventory.service.StockService;
import org.springframework.web.bind.annotation.*;

@RestController
@RequestMapping(path = "/api/stock")
public class StockController {
    private final StockService stockService;

    public StockController(StockService stockService) {
        this.stockService = stockService;
    }

    @GetMapping(value = "/{sku}")
    public Object getStock(@PathVariable String sku) {
        return stockService.findBySku(sku);
    }

    @PutMapping("/{sku}")
    public Object updateStock(@PathVariable String sku, @RequestBody Object payload) {
        return stockService.updateStock(sku, payload);
    }

    @PatchMapping("/{sku}/reserve")
    public Object reserveStock(@PathVariable String sku) {
        return stockService.reserve(sku);
    }

    @RequestMapping(method = RequestMethod.HEAD, path = "/health")
    public void head() {
        stockService.health();
    }
}
