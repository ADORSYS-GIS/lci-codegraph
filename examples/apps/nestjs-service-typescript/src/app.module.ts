import { Module } from '@nestjs/common';
import { NotificationController } from './notifications/notification.controller';
import { NotificationService } from './notifications/notification.service';
import { NotificationRepository } from './notifications/notification.repository';

@Module({
  controllers: [NotificationController],
  providers: [NotificationService, NotificationRepository],
})
export class AppModule {}
