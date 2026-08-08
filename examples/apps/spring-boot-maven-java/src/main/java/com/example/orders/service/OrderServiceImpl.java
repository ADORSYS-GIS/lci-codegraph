package com.example.orders.service;

import com.example.orders.repo.Order;
import com.example.orders.repo.OrderRepository;
import com.example.orders.client.ShippingClient;
import org.springframework.stereotype.Service;
import java.util.List;

@Service
public class OrderServiceImpl implements OrderService {
    private final OrderRepository orderRepository;
    private final ShippingClient shippingClient;

    public OrderServiceImpl(OrderRepository orderRepository, ShippingClient shippingClient) {
        this.orderRepository = orderRepository;
        this.shippingClient = shippingClient;
    }

    @Override
    public Order findById(Long id) {
        return orderRepository.findById(id).orElse(null);
    }

    @Override
    public List<Order> findAll() {
        return orderRepository.findAll();
    }

    @Override
    public List<Order> findByCustomerEmail(String email) {
        return orderRepository.findByCustomerEmail(email);
    }

    @Override
    public Order create(Order order) {
        Order saved = orderRepository.save(order);
        shippingClient.requestPickup(saved.getId());
        return saved;
    }

    @Override
    public void delete(Long id) {
        orderRepository.deleteById(id);
    }
}
