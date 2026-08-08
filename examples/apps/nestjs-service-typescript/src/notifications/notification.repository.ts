import { Injectable } from '@nestjs/common';

@Injectable()
export class NotificationRepository {
  private notifications = new Map<number, any>();
  private nextId = 1;

  findById(id: number) {
    return this.notifications.get(id);
  }

  insert(data: any) {
    const id = this.nextId++;
    const record = { id, ...data };
    this.notifications.set(id, record);
    return record;
  }

  remove(id: number) {
    this.notifications.delete(id);
    return { success: true };
  }
}
