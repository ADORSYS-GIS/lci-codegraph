package com.example.orders.service;

import com.example.orders.repo.Order;
import java.util.List;

public interface OrderService {
    Order findById(Long id);
    List<Order> findAll();
    Order create(Order order);
    List<Order> findByCustomerEmail(String email);

    void delete(Long id);
}
