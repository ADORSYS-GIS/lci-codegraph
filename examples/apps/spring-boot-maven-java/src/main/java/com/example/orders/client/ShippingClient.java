package com.example.orders.client;

import org.springframework.cloud.openfeign.FeignClient;
import org.springframework.web.bind.annotation.PostMapping;
import org.springframework.web.bind.annotation.GetMapping;
import org.springframework.web.bind.annotation.PathVariable;

@FeignClient(name = "shipping-service")
public interface ShippingClient {
    @PostMapping("/shipments/{orderId}/pickup")
    void requestPickup(@PathVariable Long orderId);

    @GetMapping("/shipments/{orderId}/track")
    String trackShipment(@PathVariable Long orderId);
}
