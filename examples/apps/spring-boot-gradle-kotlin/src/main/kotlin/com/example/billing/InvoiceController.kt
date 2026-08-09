package com.example.billing

import org.springframework.web.bind.annotation.*

@RestController
@RequestMapping("/api/invoices")
class InvoiceController(private val invoiceService: InvoiceService) {
    @GetMapping("/{id}")
    fun getInvoice(@PathVariable id: Long): Invoice? {
        return invoiceService.findById(id)
    }

    @PostMapping
    fun createInvoice(@RequestBody invoice: Invoice): Invoice {
        return invoiceService.save(invoice)
    }
}
