package com.example.billing

import org.springframework.data.jpa.repository.JpaRepository
import org.springframework.stereotype.Repository
import jakarta.persistence.Entity
import jakarta.persistence.GeneratedValue
import jakarta.persistence.Id

@Repository
interface InvoiceRepository : JpaRepository<Invoice, Long> {
    fun findByCustomerId(customerId: Long): List<Invoice>
}

@Entity
data class Invoice(
    @Id @GeneratedValue val id: Long = 0,
    val customerId: Long = 0,
    val amount: Double = 0.0
)
