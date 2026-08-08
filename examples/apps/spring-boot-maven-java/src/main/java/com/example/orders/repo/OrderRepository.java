package com.example.orders.repo;

import org.springframework.data.jpa.repository.JpaRepository;
import org.springframework.stereotype.Repository;
import java.util.List;

/**
 * Declares only DERIVED QUERIES — the methods a real Spring Data repository actually spells out.
 * Everything else it offers (save, findById, findAll, deleteById, ...) is inherited from
 * JpaRepository and appears in no source file in this project.
 *
 * That distinction is visible in this app's graph.json, and it is the honest limit of a syntactic
 * extractor: a call to findByCustomerEmail resolves here, because the declaration is right below;
 * a call to the inherited save() resolves to nothing, because there is nothing in this checkout to
 * resolve it TO. See this app's README.
 */
@Repository
public interface OrderRepository extends JpaRepository<Order, Long> {
    List<Order> findByCustomerEmail(String customerEmail);

    List<Order> findByTotalPriceGreaterThan(double threshold);
}
