package com.example.inventory.api;

import com.example.inventory.service.StockService;
import org.springframework.stereotype.Controller;
import org.springframework.web.bind.annotation.GetMapping;

@Controller
public class AdminController {
    private final StockService stockService;

    public AdminController(StockService stockService) {
        this.stockService = stockService;
    }

    @GetMapping({"/admin/stock", "/admin/inventory"})
    public String adminView() {
        stockService.getAdminStats();
        return "admin-view";
    }
}
