package com.example.billing

import org.springframework.stereotype.Service

interface InvoiceService {
    fun findById(id: Long): Invoice?
    fun save(invoice: Invoice): Invoice
}

@Service
class InvoiceServiceImpl(private val repo: InvoiceRepository) : InvoiceService {
    override fun findById(id: Long): Invoice? = repo.findById(id).orElse(null)
    override fun save(invoice: Invoice): Invoice = repo.save(invoice)
}
