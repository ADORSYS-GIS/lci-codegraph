import { Controller, Get, Post, Delete, Param, Body } from '@nestjs/common';
import { NotificationService } from './notification.service';

@Controller('notifications')
export class NotificationController {
  constructor(private readonly service: NotificationService) {}

  @Get(':id')
  async getNotification(@Param('id') id: string) {
    return this.service.findById(parseInt(id));
  }

  @Post()
  async createNotification(@Body() data: any) {
    return this.service.create(data);
  }

  @Delete(':id')
  async deleteNotification(@Param('id') id: string) {
    return this.service.delete(parseInt(id));
  }
}
